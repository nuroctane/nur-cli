import { afterEach, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import native from './fixtures/shared-libs-resolved-reads-public.json';
import { execFileSync } from 'node:child_process';
import { createSharedLibsFixture, fixtureGit } from './helpers/shared-libs-eval-fixture';
import { seedPathReviewPrerequisites, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt } from './helpers/shared-libs-path-fixture';

const source = fs.readFileSync(path.join(import.meta.dir, 'skill-e2e-shared-libs-paths.test.ts'), 'utf8');
const detectorStart = source.indexOf('function sourceReadTrace(');
const callbackStart = source.indexOf('async function exerciseEligibility(');
const callbackEnd = source.indexOf('\ndescribeE2E(', callbackStart);
if (detectorStart < 0 || callbackStart <= detectorStart || callbackEnd <= callbackStart) throw new Error('Missing production detector or callback');
const transpiler = new Bun.Transpiler({ loader: 'ts' });
const detect = new Function('fs', 'path', `${transpiler.transformSync(source.slice(detectorStart, callbackStart))}; return sourceReadTrace;`)(fs, path);
const roots: string[] = [];
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); });
const readId = 'toolu_0165HzBNP4ydttNR7XzQBxnK';
const nativeRead = native.matching_results.find(row => row.tool_use_id === readId)!;
const aliases = ['src/retry-route.ts', 'src/retry-alias/retry.ts'];
const targets = ['.fixture/first-party/direct-route.ts', '.fixture/first-party/routes/retry.ts'];

function fixture() {
  const f = createSharedLibsFixture('resolved-read');
  const { root, repo } = f;
  roots.push(root);
  for (const file of ['src/retry-worker.ts', 'lib/retry-after.ts', ...targets]) {
    const body = nativeRead.content.split(`=== ${file} ===\n`)[1]?.split('\n=== ')[0];
    if (!body) throw new Error(`Missing native file contents: ${file}`);
    fs.mkdirSync(path.dirname(path.join(repo, file)), { recursive: true });
    fs.writeFileSync(path.join(repo, file), body);
  }
  fs.symlinkSync('../.fixture/first-party/direct-route.ts', path.join(repo, aliases[0]));
  fs.symlinkSync('../.fixture/first-party/routes', path.join(repo, 'src/retry-alias'));
  return f;
}

function capture() {
  const tools = structuredClone(native.tools);
  const events = tools.flatMap(tool => {
    const output = native.matching_results.find(row => row.tool_use_id === tool.id);
    return [{ type: 'assistant', message: { content: [{ type: 'tool_use', ...tool }] } },
      ...(output ? [{ type: 'user', message: { content: [structuredClone(output)] } }] : [])];
  });
  return { exitReason: 'success', output: '', events,
    toolCalls: tools.map(tool => ({ tool: tool.name, input: tool.input, output: '' })) };
}

test('fixture Git and child commands share a platform-safe empty global config', () => {
  const f = createSharedLibsFixture('git-config');
  roots.push(f.root);
  const config = f.env.GIT_CONFIG_GLOBAL;
  if (process.platform === 'win32') {
    expect(fs.statSync(config).isFile()).toBe(true);
    expect(path.dirname(fs.realpathSync(config))).toBe(fs.realpathSync(f.root));
  } else {
    expect(config).toBe(os.devNull);
    expect(fs.existsSync(path.join(f.root, 'gitconfig'))).toBe(false);
  }
  expect(fs.readFileSync(config, 'utf8')).toBe('');
  expect(f.env.GIT_CONFIG_NOSYSTEM).toBe('1');
  expect(fixtureGit(f, 'config', '--global', '--list')).toBe('');
  expect(fixtureGit(f, 'config', '--local', '--get', 'user.name')).toBe('Shared Libs Fixture');
  expect(fixtureGit(f, 'rev-parse', 'HEAD')).toBe(f.tip);
  const args = ['config', '--global', '--show-origin', '--list'];
  expect(execFileSync(Bun.which('git') || 'git', args, {
    cwd: f.repo, env: { ...process.env, ...f.env }, encoding: 'utf8', timeout: 10_000,
  }).trim()).toBe('');
  if (process.platform === 'win32') {
    fs.writeFileSync(config, '[fixture]\n\tconfig = owned\n');
    expect(fixtureGit(f, 'config', '--global', '--get', 'fixture.config')).toBe('owned');
    expect(execFileSync(Bun.which('git') || 'git', args, {
      cwd: f.repo, env: { ...process.env, ...f.env }, encoding: 'utf8', timeout: 10_000,
    }).trim()).toContain('fixture.config=owned');
  }
});

test.each(['win32', 'linux', 'darwin'])('worktree fingerprint launches its Bash script on %s', platform => {
  const helper = fs.readFileSync(path.join(import.meta.dir, 'helpers/shared-libs-eval-fixture.ts'), 'utf8');
  const start = helper.indexOf('export function fixtureWorkingTree(');
  const end = helper.indexOf('\nexport async function runSharedCapture(', start);
  expect(start).toBeGreaterThan(0);
  expect(end).toBeGreaterThan(start);
  const calls: any[] = [];
  const paths = platform === 'win32' ? path.win32 : path.posix;
  const root = platform === 'win32' ? 'C:\\fixture root\\gstack' : '/fixture root/gstack';
  const script = paths.join(root, 'bin/gstack-wtree');
  const launch = new Function('execFileSync', 'path', 'SHARED_LIBS_ROOT', 'process',
    `${transpiler.transformSync(helper.slice(start, end).replace('export function', 'function'))}; return fixtureWorkingTree;`)(
    (command: string, args: string[], options: any) => { calls.push({ command, args, options }); return 'tree-hash\n'; },
    paths, root, { platform, env: { PATH: 'host-path', HOME: 'host-home' } });
  const f = { repo: paths.join(root, 'repo'), env: { GSTACK_HOME: 'isolated-state', PATH: 'fixture-path' } };
  expect(launch(f)).toBe('tree-hash');
  expect(calls).toEqual([{
    command: platform === 'win32' ? 'bash' : script,
    args: platform === 'win32' ? [script] : [],
    options: { cwd: f.repo, encoding: 'utf8', timeout: 30_000,
      env: { PATH: 'host-path', HOME: 'host-home', GSTACK_HOME: 'isolated-state' } },
  }]);
});

test('the retained native resolved-path read proves both current authored callers', () => {
  const f = fixture();
  const result = capture();
  const reads = detect(result, f, aliases);
  for (const alias of aliases) expect(reads).toContain(alias);
  for (const target of targets) expect(fs.readFileSync(path.join(f.repo, target), 'utf8')).toContain('changed after the prior decision');
});

test.each(['missing-result', 'failed-result', 'wrong-result-id', 'metadata-only', 'stale-content', 'assistant-only'])('%s cannot prove resolved source reads', kind => {
  const f = fixture();
  const result: any = capture();
  const event = result.events.find((event: any) => event.message.content[0].tool_use_id === readId);
  const block = event.message.content[0];
  if (kind === 'missing-result') result.events = result.events.filter((candidate: any) => candidate !== event);
  if (kind === 'failed-result') block.is_error = true;
  if (kind === 'wrong-result-id') block.tool_use_id = 'unrelated';
  if (kind === 'metadata-only') block.content = 'Both target files exist and are 738 bytes.';
  if (kind === 'stale-content') block.content = block.content.replaceAll('// Authored caller changed after the prior decision (symlinks).', '');
  if (kind === 'assistant-only') event.type = 'assistant';
  const reads = detect(result, f, aliases);
  for (const alias of aliases) expect(reads).not.toContain(alias);
});

test('a canonical Read result accepts native line prefixes but still requires current contents', () => {
  const f = fixture();
  const content = fs.readFileSync(path.join(f.repo, targets[0]), 'utf8');
  const input = { file_path: path.join(f.repo, targets[0]) };
  const block = { type: 'tool_result', tool_use_id: 'read-target', content: content.split('\n').map((line, index) => `${index + 1}→${line}`).join('\n') };
  const result = { toolCalls: [{ tool: 'Read', input }], events: [
    { type: 'assistant', message: { content: [{ type: 'tool_use', id: 'read-target', name: 'Read', input }] } },
    { type: 'user', message: { content: [block] } },
  ] };
  expect(detect(result, f, aliases)).toContain(aliases[0]);
  expect(detect(result, f, aliases)).not.toContain(aliases[1]);
  block.content = '';
  expect(detect(result, f, aliases)).not.toContain(aliases[0]);
});

test('resolved aliases cannot grant credit for targets outside the fixture repository', () => {
  const f = fixture();
  const outside = path.join(f.root, 'outside.ts');
  fs.writeFileSync(outside, fs.readFileSync(path.join(f.repo, targets[0])));
  fs.unlinkSync(path.join(f.repo, aliases[0]));
  fs.symlinkSync(outside, path.join(f.repo, aliases[0]));
  const result: any = capture();
  for (const tool of result.toolCalls) if (tool.tool === 'Bash') tool.input.command = tool.input.command.replaceAll(targets[0], outside);
  for (const event of result.events) for (const block of event.message.content) {
    if (block.type === 'tool_use' && block.name === 'Bash') block.input.command = block.input.command.replaceAll(targets[0], outside);
  }
  expect(detect(result, f, aliases)).not.toContain(aliases[0]);
  expect(detect(result, f, aliases)).toContain(aliases[1]);
});

test.each(['.backup', '/foreign-root/'])('similarly named read targets cannot acquire alias credit: %s', spelling => {
  const f = fixture();
  const result: any = capture();
  const tool = result.events.flatMap((event: any) => event.message.content).find((block: any) => block.id === readId);
  for (const target of targets) tool.input.command = tool.input.command.replaceAll(target, spelling === '.backup' ? target + spelling : spelling + target);
  const reads = detect(result, f, aliases);
  for (const alias of aliases) expect(reads).not.toContain(alias);
});

test('a Read result from another repository cannot prove an identically named target', () => {
  const f = fixture();
  const input = { file_path: path.join(f.root, 'another-repo', targets[0]) };
  const result = { toolCalls: [{ tool: 'Read', input }], events: [
    { type: 'assistant', message: { content: [{ type: 'tool_use', id: 'foreign-read', name: 'Read', input }] } },
    { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 'foreign-read', content: fs.readFileSync(path.join(f.repo, targets[0]), 'utf8') }] } },
  ] };
  expect(detect(result, f, aliases)).not.toContain(aliases[0]);
});

test.each(['Read', 'Bash'])('direct alias reads retain the existing %s contract', tool => {
  const f = fixture();
  const result = { toolCalls: [{ tool, input: tool === 'Read' ? { file_path: aliases[0] } : { command: `cat ${aliases[0]}` } }] };
  expect(detect(result, f, aliases)).toContain(aliases[0]);
});

test.each(['valid', 'missing-result', 'failed-result', 'wrong-result-id', 'metadata-only', 'stale-content',
  'assistant-only', 'suffix', 'foreign-root', 'outside-target'])('Windows native-path replay preserves read evidence: %s', kind => {
  const f = fixture();
  const windowsRoot = 'C:\\shared-libs-fixture';
  const nativePath = (file: string) => path.resolve(f.root, ...path.win32.relative(windowsRoot, file).split('\\'));
  const windowsFs = {
    realpathSync: (file: string) => path.win32.resolve(windowsRoot, path.relative(f.root, fs.realpathSync(nativePath(file)))),
    readFileSync: (file: string, encoding: BufferEncoding) => fs.readFileSync(nativePath(file), encoding),
  };
  const windowsDetect = new Function('fs', 'path', `${transpiler.transformSync(source.slice(detectorStart, callbackStart))}; return sourceReadTrace;`)(windowsFs, path.win32);
  const result: any = capture();
  const event = result.events.find((row: any) => row.message.content[0].tool_use_id === readId);
  const block = event.message.content[0];
  if (kind === 'missing-result') result.events = result.events.filter((row: any) => row !== event);
  if (kind === 'failed-result') block.is_error = true;
  if (kind === 'wrong-result-id') block.tool_use_id = 'unrelated';
  if (kind === 'metadata-only') block.content = 'Both target files exist and are 738 bytes.';
  if (kind === 'stale-content') block.content = block.content.replaceAll('// Authored caller changed after the prior decision (symlinks).', '');
  if (kind === 'assistant-only') event.type = 'assistant';
  if (kind === 'suffix' || kind === 'foreign-root') {
    const tool = result.events.flatMap((row: any) => row.message.content).find((row: any) => row.id === readId);
    for (const target of targets) tool.input.command = tool.input.command.replaceAll(target,
      kind === 'suffix' ? `${target}.backup` : `/foreign-root/${target}`);
  }
  if (kind === 'outside-target') {
    const outside = path.join(f.root, 'outside.ts');
    fs.writeFileSync(outside, fs.readFileSync(path.join(f.repo, targets[0])));
    fs.unlinkSync(path.join(f.repo, aliases[0]));
    fs.symlinkSync(outside, path.join(f.repo, aliases[0]));
    const tool = result.events.flatMap((row: any) => row.message.content).find((row: any) => row.id === readId);
    tool.input.command = tool.input.command.replaceAll(targets[0], 'C:/shared-libs-fixture/outside.ts');
  }
  const reads = windowsDetect(result, { repo: path.win32.join(windowsRoot, 'repo') }, aliases);
  if (kind === 'valid') for (const alias of aliases) expect(reads).toContain(alias);
  else if (kind === 'outside-target') {
    expect(reads).not.toContain(aliases[0]);
    expect(reads).toContain(aliases[1]);
  } else for (const alias of aliases) expect(reads).not.toContain(alias);
});
test.each([false, true])('the actual eligibility callback consumes the read detector result (missing output=%s)', async missingOutput => {
  const f = fixture();
  const result: any = capture();
  if (missingOutput) result.events = result.events.filter((event: any) => event.message.content[0].tool_use_id !== readId);
  const resumed = seedPathReviewPrerequisites(f);
  const prerequisiteOutput = execFileSync('bash', ['-c', resumed.checkCommand], {
    cwd: f.repo, env: { ...process.env, ...f.env }, encoding: 'utf8', timeout: 30_000,
  });
  const finish = result.events.findIndex((event: any) => event.type === 'assistant'
    && event.message.content.some((block: any) => block.input?.command?.includes('--finish')));
  expect(finish).toBeGreaterThan(0);
  result.events.splice(finish, 0,
    { type: 'assistant', message: { content: [{ type: 'tool_use', name: 'Bash', id: 'prerequisites', input: { command: resumed.checkCommand } }] } },
    { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 'prerequisites', content: prerequisiteOutput }] } });
  const rows: any[] = [];
  let detectorCalls = 0;
  const exercise = new Function('deps', `const { captures, preparePathEligibilityFixture, fs, path,
    reviewLifecycleInstructions, reviewRevalidationPrompt, runSharedInteractive, readRequests,
    toolCommandTrace, sourceReadTrace, fixtureWorkingTree, reviewRecords, expect, CAPTURE_LONG_MS,
    checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt } = deps;
    ${transpiler.transformSync(source.slice(callbackStart, callbackEnd))}; return exerciseEligibility;`)({
    captures: { runAttempt: (_name: string, _kinds: string[], _timeout: number, work: any) => work({ add: (_kind: string, row: any) => rows.push(row) }) },
    preparePathEligibilityFixture: () => ({ fixture: f, resumed, sourcePaths: aliases, beforeTree: 'tree', current: { evidence_paths: ['src/retry-worker.ts', 'lib/retry-after.ts', ...aliases] } }),
    fs, path, expect, CAPTURE_LONG_MS: 600_000,
    reviewLifecycleInstructions: () => 'read the authored sources', reviewRevalidationPrompt: () => 'revalidate',
    runSharedInteractive: async () => ({ result, questions: [{}] }), readRequests: () => [],
    toolCommandTrace: (value: any) => value.toolCalls.filter((call: any) => call.tool === 'Bash').map((call: any) => call.input.command),
    sourceReadTrace: (...args: any[]) => { detectorCalls++; return detect(...args); },
    fixtureWorkingTree: () => 'tree',
    checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt,
    reviewRecords: () => [{ skill: 'review' }, { skill: 'review', status: 'clean', issues_found: 0, completed: true, converged: true,
      review_binding: { state: 'verified' }, findings: [{ advisory: true, action: 'skipped', helper_target: { path: 'lib/retry-after.ts', symbol: 'retrySeconds' },
        fingerprint: `shared-libs:${'a'.repeat(64)}`, evidence_paths: ['src/retry-worker.ts', ...aliases], snapshot_covered_paths: ['src/retry-worker.ts', 'lib/retry-after.ts'] }] }],
  });
  if (missingOutput) await expect(exercise('shared-libs-review-path-eligibility', ['symlinks'])).rejects.toThrow();
  else await exercise('shared-libs-review-path-eligibility', ['symlinks']);
  expect(detectorCalls).toBe(1);
  expect(rows).toHaveLength(1);
  expect(rows[0].passed).toBe(!missingOutput);
  expect(fs.existsSync(f.root)).toBe(false);
});
