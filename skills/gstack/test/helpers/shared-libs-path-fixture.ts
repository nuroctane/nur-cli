/** Real filesystem boundaries for the shared-code advisory eligibility eval. */
import * as fs from 'node:fs';
import * as path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { sharedLibsFingerprint } from '../../lib/review-evidence';
import {
  SHARED_LIBS_ROOT, commitFixture, createSharedLibsFixture, fixtureGit, fixtureWrite,
  fixtureWorkingTree, seedReviewSources, shellQuote, snapshotFixture, type SharedLibsFixture, type SharedReviewResume, type SharedReviewStageActor,
} from './shared-libs-eval-fixture';

export type PathEligibilityCase = 'symlinks' | 'submodule' | 'ignored' | 'legacy' | 'assume-unchanged' | 'skip-worktree' | 'removed-filter';
export interface PathEligibilityFixture {
  fixture: SharedLibsFixture;
  current: Record<string, any>;
  beforeTree: string;
  sourcePaths: string[];
  rawPaths: string[];
  resumed: SharedReviewResume;
}

function pathReviewState(f: SharedLibsFixture) {
  const root = fs.realpathSync(f.root), repo = fs.realpathSync(f.repo), state = fs.realpathSync(f.state);
  if (repo !== path.join(root, 'repo') || state !== path.join(root, 'state')) throw new Error('Foreign path fixture state');
  const raw = Object.fromEntries(Object.entries(snapshotFixture(repo)).filter(([file, value]) =>
    !file.split(path.sep).includes('.git') || /(?:^|\/)(?:config|info\/(?:attributes|exclude))$/.test(file)
      || path.basename(file) === '.git' && !value.startsWith('dir:')));
  return { root, repo, state, branch: fixtureGit(f, 'symbolic-ref', '--short', 'HEAD'),
    head: fixtureGit(f, 'rev-parse', 'HEAD'), base: fixtureGit(f, 'rev-parse', 'origin/main'),
    wtree: fixtureWorkingTree(f), index: fixtureGit(f, 'ls-files', '--stage', '-v'), raw };
}

export function seedPathReviewPrerequisites(f: SharedLibsFixture): SharedReviewResume {
  const input = path.join(f.root, 'resumed-review-prerequisites.json');
  const changedLines = fixtureGit(f, 'diff', '--numstat', 'origin/main').split('\n')
    .reduce((sum, line) => sum + line.split('\t').slice(0, 2).reduce((n, value) => n + Number(value || 0), 0), 0);
  if (!Number.isFinite(changedLines) || changedLines >= 50) throw new Error('Resumed path fixture requires a tiny no-edit diff');
  const context = { kind: 'synthetic-path-review-prerequisites', synthetic: true, native_coverage: false,
    binding: pathReviewState(f),
    qa: { settled: true, required_probes: [{ id: 'retry-contract', status: 'passed',
      result: 'Synthetic fixture input: Retry-After seconds/date parsing, ceiling and fallback probes passed.' }], findings: [] },
    native_adversarial: { settled: true, status: 'completed', findings: [],
      result: 'Synthetic fixture input: native adversarial review returned no findings.' },
    structured_review: { required: false, reason: 'Tiny diff; no full-review, structured-review or P1 override requested.' } };
  fs.writeFileSync(input, JSON.stringify(context, null, 2) + '\n', { mode: 0o600 });
  return { input, checkCommand: `bun ${shellQuote(path.join(SHARED_LIBS_ROOT, 'test/helpers/shared-libs-path-fixture.ts'))} --check-review-prerequisites ${shellQuote(input)}` };
}

export function checkPathReviewPrerequisites(f: SharedLibsFixture, input: string) {
  try {
    if (input !== path.join(f.root, 'resumed-review-prerequisites.json')) throw new Error('Foreign prerequisite file');
    const text = fs.readFileSync(input, 'utf8'), context = JSON.parse(text);
    const current = JSON.stringify(context.binding) === JSON.stringify(pathReviewState(f));
    const settled = current && context.kind === 'synthetic-path-review-prerequisites'
      && context.synthetic === true && context.native_coverage === false
      && context.qa?.settled === true && Array.isArray(context.qa.required_probes) && context.qa.required_probes.length === 1
      && context.qa.required_probes.every((probe: any) => probe.id === 'retry-contract' && probe.status === 'passed'
        && typeof probe.result === 'string' && probe.result.length > 0)
      && Array.isArray(context.qa.findings) && context.qa.findings.length === 0
      && context.native_adversarial?.settled === true && context.native_adversarial.status === 'completed'
      && Array.isArray(context.native_adversarial.findings) && context.native_adversarial.findings.length === 0
      && typeof context.native_adversarial.result === 'string' && context.native_adversarial.result.length > 0
      && context.structured_review?.required === false;
    return { synthetic: true, native_coverage: false, settled, current,
      input_sha256: createHash('sha256').update(text).digest('hex'), context };
  } catch {
    return { synthetic: true, native_coverage: false, settled: false, current: false };
  }
}

export function hasPathReviewPrerequisiteReceipt(events: any[], command: string, expected: ReturnType<typeof checkPathReviewPrerequisites>): boolean {
  if (!expected.settled) return false;
  const calls = new Set<string>();
  let verified = false;
  for (const event of events) for (const block of Array.isArray(event.message?.content) ? event.message.content : []) {
    if (event.type === 'assistant' && block.type === 'tool_use' && block.name === 'Bash') {
      if (block.input?.command === command) calls.add(block.id);
      if (String(block.input?.command).includes('--finish')) return verified;
    }
    if (event.type !== 'user' || block.type !== 'tool_result' || block.is_error === true || !calls.has(block.tool_use_id)) continue;
    const text = typeof block.content === 'string' ? block.content : Array.isArray(block.content)
      ? block.content.filter((part: any) => part.type === 'text').map((part: any) => part.text).join('\n') : '';
    try { verified = JSON.stringify(JSON.parse(text)) === JSON.stringify(expected); } catch { verified = false; }
  }
  return false;
}

export function createLifecyclePrerequisiteActor(f: SharedLibsFixture): SharedReviewStageActor {
  const directory = path.join(fs.realpathSync(f.root), 'synthetic-stage-receipts');
  fs.mkdirSync(directory, { mode: 0o700 });
  const output = path.join(directory, 'current.json');
  const actorCommand = `cat ${shellQuote(output)}`;
  let state = pathReviewState(f), generation = 0;
  const isolation = [state.root, state.repo, state.state];
  const issued = new Map<string, { text: string; file: string; generation: number; settled: boolean }>();
  const finishes = new Map<string, { command: string; generation: number }>();
  const observe = () => {
    const current = pathReviewState(f);
    if (JSON.stringify([current.root, current.repo, current.state]) !== JSON.stringify(isolation)) throw new Error('Rebound synthetic stage fixture');
    if (JSON.stringify(current) !== JSON.stringify(state)) { generation++; state = current; }
    return current;
  };
  const beforeTool: SharedReviewStageActor['hooks']['PreToolUse'][number]['hooks'][number] = async (input, toolUseID) => {
    if (input.hook_event_name !== 'PreToolUse') return {};
    const current = observe();
    const command = input.tool_name === 'Bash' ? (input.tool_input as any)?.command : undefined;
    if (command === actorCommand) {
      const id = input.tool_use_id;
      if (fs.realpathSync(input.cwd) !== current.repo || !id || toolUseID !== undefined && id !== toolUseID || issued.has(id)
        || fs.realpathSync(directory) !== directory || fs.existsSync(output) && fs.lstatSync(output).isSymbolicLink()) {
        return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny', permissionDecisionReason: 'Invalid synthetic stage invocation' } };
      }
      const evidence_paths = ['src/retry-worker.ts', 'src/retry-route.ts', 'lib/retry-after.ts'];
      const fingerprint = sharedLibsFingerprint({ evidence_paths, helper_target: { path: 'lib/retry-after.ts', symbol: 'retrySeconds' } });
      const changedLines = fixtureGit(f, 'diff', '--numstat', 'origin/main').split('\n')
        .reduce((sum, line) => sum + line.split('\t').slice(0, 2).reduce((n, value) => n + Number(value || 0), 0), 0);
      const tinyDiff = Number.isFinite(changedLines) && changedLines < 50;
      const authoredEvidence = !!fingerprint && evidence_paths.every(source => {
        const file = path.join(current.repo, source);
        try { return fs.realpathSync(file) === file && fs.lstatSync(file).isFile() && fs.statSync(file).size > 0; }
        catch { return false; }
      });
      const supported = tinyDiff && authoredEvidence;
      const receipt = { kind: 'synthetic-lifecycle-stage-result', id: randomUUID(), tool_use_id: id,
        synthetic: true, native_coverage: false, generation, binding: current,
        deterministic_checks: { fixture_isolation: true, tiny_diff: tinyDiff, authored_evidence: authoredEvidence, fingerprint },
        qa: { settled: supported, required_probes: [{ id: 'retry-contract', status: supported ? 'passed' : 'blocked',
          result: 'Simulated fixture QA outcome, not execution of target tests.' }], findings: [] },
        native_adversarial: { settled: supported, status: supported ? 'completed' : 'blocked', findings: [],
          result: 'Simulated fixture adversarial outcome, not an actual native review.' },
        ...(tinyDiff ? { structured_review: { required: false, reason: 'Tiny diff; no full-review override in this fixture.' } } : {}),
        settled: supported };
      const text = JSON.stringify(receipt), file = path.join(directory, `${receipt.id}.json`);
      fs.writeFileSync(file, text, { flag: 'wx', mode: 0o600 });
      fs.writeFileSync(output, text, { mode: 0o600 });
      issued.set(id, { text, file, generation, settled: supported });
      return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'allow' } };
    }
    if (typeof command === 'string' && command.includes('gstack-review-log') && command.includes('--finish')) {
      finishes.set(input.tool_use_id, { command, generation });
    }
    return {};
  };
  return { actorCommand, hooks: { PreToolUse: [{ hooks: [beforeTool] }] },
    history: () => [...issued.values()].map(receipt => JSON.parse(receipt.text)),
    verify(events) {
      try {
        observe();
        if (!issued.size || [...issued.values()].some(receipt => fs.readFileSync(receipt.file, 'utf8') !== receipt.text)) return false;
        const calls = new Map<string, string>();
        let consumed: string | undefined, final: string | undefined, finalReceipt: string | undefined, finished = false;
        for (const event of events) for (const block of Array.isArray(event.message?.content) ? event.message.content : []) {
          if (event.type === 'assistant' && block.type === 'tool_use' && block.name === 'Bash') {
            calls.set(block.id, block.input?.command);
            if (finishes.get(block.id)?.command === block.input?.command) { final = block.id; finalReceipt = consumed; finished = false; }
          }
          if (event.type !== 'user' || block.type !== 'tool_result') continue;
          if (block.tool_use_id === final) finished = block.is_error !== true;
          if (calls.get(block.tool_use_id) !== actorCommand) continue;
          const receipt = issued.get(block.tool_use_id);
          const text = typeof block.content === 'string' ? block.content : Array.isArray(block.content)
            ? block.content.filter((part: any) => part.type === 'text').map((part: any) => part.text).join('\n') : '';
          consumed = receipt && receipt.settled && block.is_error !== true
            && JSON.stringify(JSON.parse(text)) === receipt.text ? block.tool_use_id : undefined;
        }
        const receipt = finalReceipt ? issued.get(finalReceipt) : undefined;
        return !!receipt && finished && finalReceipt === [...issued.keys()].at(-1)
          && final === [...finishes.keys()].at(-1) && receipt.generation === generation && finishes.get(final!)?.generation === generation;
      } catch { return false; }
    } };
}

function seedBoundSkip(f: SharedLibsFixture, finding: Record<string, any>): void {
  const logger = path.join(SHARED_LIBS_ROOT, 'bin/gstack-review-log');
  const options = { cwd: f.repo, env: { ...process.env, ...f.env }, encoding: 'utf8' as const, timeout: 30_000 };
  const token = execFileSync(logger, ['--start', 'review'], { ...options, timeout: 30_000 }).trim();
  execFileSync(logger, [JSON.stringify({
    skill: 'review', timestamp: new Date().toISOString(), status: 'clean',
    issues_found: 0, critical: 0, informational: 0, quality_score: 10,
    findings: [finding], completed: true, converged: true, cycles: 0,
  }), '--finish', token], { ...options, timeout: 30_000 });
}

/** The prior row is written by the real logger; no caller-created binding is trusted. */
export function preparePathEligibilityFixture(kind: PathEligibilityCase): PathEligibilityFixture {
  const fixture = createSharedLibsFixture(`path-${kind}`);
  const f = fixture;
  try {
    seedReviewSources(f);
    fixtureWrite(f, 'src/retry-worker.ts', fs.readFileSync(path.join(f.repo, 'src/retry-worker.ts'), 'utf8')
      .replace('const unusedRetryDiagnostic = "unused";\n', ''));
    const parser = fs.readFileSync(path.join(f.repo, 'src/retry-route.ts'), 'utf8');
    let sourcePaths = ['src/retry-route.ts'];
    let rawPaths: string[] = [];

    if (kind === 'symlinks') {
      // Both a file symlink and a symlinked ancestor lead to authored runtime source.
      // Their targets are ignored, so raw changes cannot alter the parent Git tree.
      fixtureWrite(f, '.fixture/first-party/direct-route.ts', parser);
      fixtureWrite(f, '.fixture/first-party/routes/retry.ts', parser);
      fs.unlinkSync(path.join(f.repo, 'src/retry-route.ts'));
      fs.symlinkSync('../.fixture/first-party/direct-route.ts', path.join(f.repo, 'src/retry-route.ts'));
      fs.symlinkSync('../.fixture/first-party/routes', path.join(f.repo, 'src/retry-alias'));
      sourcePaths = ['src/retry-route.ts', 'src/retry-alias/retry.ts'];
      rawPaths = ['.fixture/first-party/direct-route.ts', '.fixture/first-party/routes/retry.ts'];
    } else if (kind === 'submodule') {
      // Build and clone a local first-party repository; no external Git service is needed.
      const moduleOrigin = path.join(f.root, 'module-origin');
      fs.mkdirSync(moduleOrigin);
      const moduleGit = (...args: string[]) => execFileSync(Bun.which('git') || 'git', args, {
        cwd: moduleOrigin, env: { ...process.env, ...f.env }, encoding: 'utf8', timeout: 30_000,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      moduleGit('init', '-b', 'main');
      moduleGit('config', 'user.name', 'First-party Module Fixture');
      moduleGit('config', 'user.email', 'module@example.invalid');
      fs.writeFileSync(path.join(moduleOrigin, 'retry-route.ts'), parser);
      fs.writeFileSync(path.join(moduleOrigin, 'README.md'), '# First-party retry runtime\nOwned and authored by this application team. The parent application bundles this module with its root lib/ helpers in the same runtime; it is not deployed independently.\n');
      moduleGit('add', 'retry-route.ts', 'README.md');
      moduleGit('commit', '-m', 'implement retry route');
      fixtureGit(f, '-c', 'protocol.file.allow=always', 'submodule', 'add', moduleOrigin, 'modules/retry');
      fixtureGit(f, 'config', '--file', '.gitmodules', 'submodule.modules/retry.url', 'https://github.com/fixture/retry-module.git');
      fixtureWrite(f, 'src/retry-route.ts', "export { retrySeconds } from '../lib/retry-after';\n");
      sourcePaths = ['modules/retry/retry-route.ts'];
      rawPaths = sourcePaths;
    } else if (kind === 'ignored') {
      fixtureWrite(f, '.gitignore', fs.readFileSync(path.join(f.repo, '.gitignore'), 'utf8') + 'runtime-local/\n');
      fixtureWrite(f, 'runtime-local/retry-route.ts', parser);
      fixtureWrite(f, 'src/retry-route.ts', "export { retrySeconds } from '../lib/retry-after';\n");
      sourcePaths = ['runtime-local/retry-route.ts'];
      rawPaths = sourcePaths;
    } else if (kind === 'assume-unchanged' || kind === 'skip-worktree') {
      // Set the flag only AFTER the prior capture, which fully covers ordinary raw source.
      rawPaths = sourcePaths;
    } else if (kind === 'removed-filter') {
      // Keep normalization instrumentation out of the code review diff. A
      // committed .gitattributes without a distributed driver is a real defect.
      const clean = path.join(f.root, 'normalize-filter');
      fs.writeFileSync(clean, "#!/bin/sh\nsed '/^\\/\\/ RAW-ONLY/d'\n", { mode: 0o755 });
      fixtureGit(f, 'config', 'filter.normalize.clean', shellQuote(clean));
      fs.writeFileSync(path.join(f.repo, '.git/info/attributes'), 'src/retry-route.ts filter=normalize\n');
      fixtureWrite(f, 'src/retry-route.ts', parser + '// RAW-ONLY prior source was not represented by the normalized tree\n');
    }

    if (['symlinks', 'submodule', 'ignored'].includes(kind)) {
      // These deployment boundaries predate the worker change. Introducing an
      // ignored caller or a symlink into an ignored mount in the reviewed diff
      // would create a separate distribution defect, obscuring skip eligibility.
      const worker = fs.readFileSync(path.join(f.repo, 'src/retry-worker.ts'), 'utf8');
      fixtureWrite(f, 'src/retry-worker.ts', "export { retrySeconds } from '../lib/retry-after';\n");
      const boundary = kind === 'symlinks'
        ? 'The application deployment supplies the authored route adapters under .fixture/first-party/ before loading src/retry-route.ts or src/retry-alias/retry.ts. These established symlinks intentionally point into that deployment mount. The adapters contain no relative imports today; any future shared-helper import must resolve from the actual adapter location.'
        : kind === 'ignored'
          ? 'The application deployment supplies authored optional route adapters under runtime-local/. That established mount is intentionally excluded from this repository; runtime-local/retry-route.ts is first-party application code loaded only after the mount is present.'
          : 'The application includes the first-party modules/retry submodule during deployment. Its authored route adapter and the root application helpers are bundled into the same runtime; this module is not independently deployed.';
      fixtureWrite(f, 'README.md', '# Fixture application\n\n' + boundary + '\n');
      commitFixture(f, 'establish existing first-party route deployment boundary');
      fixtureWrite(f, 'src/retry-worker.ts', worker);
    }

    const current: Record<string, any> = {
      severity: 'INFORMATIONAL', confidence: 9, advisory: true,
      path: 'src/retry-worker.ts', line: 2, category: 'shared-libs',
      summary: 'Use the established Retry-After contract in the changed worker and the authored route sources.',
      fix: 'Share the tested retrySeconds contract, preserving runtime and deployment boundaries for each caller.',
      evidence_paths: ['src/retry-worker.ts', ...sourcePaths, 'lib/retry-after.ts'],
      helper_target: { path: 'lib/retry-after.ts', symbol: 'retrySeconds' },
    };
    current.fingerprint = sharedLibsFingerprint(current);
    const prior: Record<string, any> = { ...current, action: 'skipped' };
    const proofTree = fixtureWorkingTree(f);
    prior.snapshot_covered_paths = kind === 'removed-filter' ? []
      : ['symlinks', 'submodule', 'ignored'].includes(kind)
        ? ['src/retry-worker.ts', 'lib/retry-after.ts'] : [...current.evidence_paths];
    for (const sourcePath of prior.snapshot_covered_paths) {
      const blob = execFileSync(Bun.which('git') || 'git', ['-c', 'core.fsmonitor=false', 'show', `${proofTree}:${sourcePath}`], {
        cwd: f.repo, env: { ...process.env, ...f.env }, timeout: 10_000, stdio: ['ignore', 'pipe', 'pipe'],
      });
      if (!fs.readFileSync(path.join(f.repo, sourcePath)).equals(blob)) throw new Error(`Unproven prior raw coverage: ${sourcePath}`);
    }
    if (kind === 'legacy') {
      // A genuine prior review with incomplete legacy finding metadata, not a forged binding.
      delete prior.helper_target;
      delete prior.snapshot_covered_paths;
    }
    seedBoundSkip(f, prior);
    const beforeTree = fixtureWorkingTree(f);
    if (kind === 'assume-unchanged' || kind === 'skip-worktree') {
      // gstack-wtree copies these real index flags and can miss the following raw edit.
      fixtureGit(f, 'update-index', `--${kind}`, 'src/retry-route.ts');
    }
    if (kind === 'removed-filter') {
      // The current file is now untransformed and equals the same canonical blob,
      // but the earlier review did not establish raw-source coverage for that blob.
      fixtureWrite(f, 'src/retry-route.ts', parser);
      fs.writeFileSync(path.join(f.repo, '.git/info/attributes'), 'src/retry-route.ts -filter\n');
      if (!fixtureGit(f, 'check-attr', 'filter', '--', 'src/retry-route.ts').endsWith(': unset')) {
        throw new Error('Current source must no longer have a filter');
      }
    }
    for (const rawPath of rawPaths) {
      fixtureWrite(f, rawPath, fs.readFileSync(path.join(f.repo, rawPath), 'utf8')
        + `\n// Authored caller changed after the prior decision (${kind}).\n`);
    }
    if (fixtureWorkingTree(f) !== beforeTree) throw new Error(`${kind}: fixture must retain the parent Git tree`);
    return { fixture, current, beforeTree, sourcePaths, rawPaths, resumed: seedPathReviewPrerequisites(f) };
  } catch (error) {
    fs.rmSync(f.root, { recursive: true, force: true });
    throw error;
  }
}

if (import.meta.main) {
  const [flag, input] = process.argv.slice(2);
  if (flag !== '--check-review-prerequisites' || !input || !process.env.GSTACK_HOME) process.exit(2);
  const f = { root: path.dirname(input), repo: process.cwd(), state: process.env.GSTACK_HOME,
    env: { GSTACK_HOME: process.env.GSTACK_HOME } } as SharedLibsFixture;
  console.log(JSON.stringify(checkPathReviewPrerequisites(f, input)));
}
