import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { sharedLibsSnapshotCoverage } from '../lib/review-evidence';
import { gitArgvIn } from './helpers/scratch-repo';

const root = resolve(import.meta.dir, '..');
let fixture: string;
let repo: string;
let state: string;
let env: NodeJS.ProcessEnv;

function git(...args: string[]) {
  const result = gitArgvIn(repo, args, 10_000);
  if (result.status !== 0 || result.error) throw new Error(result.error?.message ?? result.stderr.toString());
  return result.stdout.toString().trim();
}

function cli(args: string[], input?: unknown) {
  const result = spawnSync(join(root, 'bin/gstack-review-log'), args, {
    cwd: repo, env, input: input === undefined ? undefined : JSON.stringify(input),
    encoding: 'utf8', timeout: 15_000,
  });
  expect(result.status, result.stderr).toBe(0);
  return result.stdout.trim();
}

function finding() {
  return { advisory: true, severity: 'INFORMATIONAL', action: 'skipped', category: 'shared-libs',
    evidence_paths: ['src/a.ts', 'src/b.ts'], helper_target: { path: 'src/shared.ts', symbol: 'parse' } };
}

function records() {
  const output = execFileSync(join(root, 'bin/gstack-review-read'), [], {
    cwd: repo, env, encoding: 'utf8', timeout: 15_000,
  });
  return output.split('---CONFIG---')[0].trim().split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line));
}

function save(value = finding(), token = cli(['--start', 'review'])) {
  cli([JSON.stringify({ skill: 'review', status: 'clean', completed: true, converged: true,
    findings: [value] }), '--finish', token]);
  return records().at(-1);
}

function check(value = finding(), token = cli(['--start', 'review'])) {
  return JSON.parse(cli(['--check-shared-libs', token], value));
}

beforeEach(() => {
  fixture = mkdtempSync(join(tmpdir(), 'reuse-'));
  repo = join(fixture, 'repo');
  state = join(fixture, 'state');
  mkdirSync(join(repo, 'src'), { recursive: true });
  env = { ...process.env, GSTACK_HOME: state, GIT_CONFIG_NOSYSTEM: '1' };
  git('init', '-q', '-b', 'main');
  git('config', 'commit.gpgsign', 'false');
  git('config', 'core.autocrlf', 'false');
  writeFileSync(join(repo, 'src/a.ts'), 'export const a = 1;\n');
  writeFileSync(join(repo, 'src/b.ts'), 'export const b = 1;\n');
  git('add', '.');
  git('commit', '-qm', 'initial');
});

afterEach(() => { if (fixture) rmSync(fixture, { recursive: true, force: true }); });

describe('executable shared-code evidence checks', () => {
  test('logger computes identity and coverage without caller-supplied proof', () => {
    const row = save();
    expect(row.findings[0].fingerprint).toMatch(/^shared-libs:[a-f0-9]{64}$/);
    expect(row.findings[0].snapshot_covered_paths).toEqual(finding().evidence_paths);
    expect(row.review_binding.state).toBe('verified');
  });

  test('checker reads real history and the unconsumed start receipt', () => {
    save();
    const token = cli(['--start', 'review']);
    const result = check(finding(), token);
    expect(result.reusable).toBe(true);
    expect(result.review_start).toMatchObject({ skill: 'review', repo, branch: 'main', wtree: result.snapshot.wtree });
    expect(result.snapshot.covered_paths).toEqual(finding().evidence_paths);
    expect(save(finding(), token).review_binding.state).toBe('verified');
    expect(check(finding(), token).reusable).toBe(false);
  });

  test('a supplied snapshot cannot invent a prior decision', () => {
    expect(check({ ...finding(), currentSnapshot: { covered_paths: finding().evidence_paths },
      priorReview: { completed: true, converged: true }, reusable: true } as any).reusable).toBe(false);
    expect(check(finding(), '../not-a-token').reusable).toBe(false);
  });

  test('a changed secondary caller or a same-content different branch cannot reuse', () => {
    save();
    git('checkout', '-qb', 'feature/a');
    save();
    git('checkout', '-qb', 'feature-a');
    expect(check().reusable).toBe(false);
    git('checkout', '-q', 'feature/a');
    const token = cli(['--start', 'review']);
    writeFileSync(join(repo, 'src/b.ts'), 'export const b = 2;\n');
    expect(check(finding(), token).reusable).toBe(false);
  });

  test('ordinary untracked authored paths are covered without modifying the real index', () => {
    writeFileSync(join(repo, 'src/new.ts'), 'export const n = 3;\n');
    const index = readFileSync(join(repo, '.git/index'));
    const value = { ...finding(), evidence_paths: ['src/a.ts', 'src/new.ts'] };
    expect(save(value).findings[0].snapshot_covered_paths).toEqual(value.evidence_paths);
    expect(check(value).reusable).toBe(true);
    expect(readFileSync(join(repo, '.git/index'))).toEqual(index);
  });

  for (const flag of ['--assume-unchanged', '--skip-worktree']) {
    test(`${flag} cannot be forged into snapshot coverage`, () => {
      save();
      git('update-index', flag, 'src/b.ts');
      const value = { ...finding(), snapshot_covered_paths: finding().evidence_paths };
      expect(save(value).findings[0].snapshot_covered_paths).not.toContain('src/b.ts');
      writeFileSync(join(repo, 'src/b.ts'), 'export const hidden = true;\n');
      expect(check(value).reusable).toBe(false);
    });
  }

  for (const attribute of ['filter=collapse', 'working-tree-encoding=UTF-8', 'ident', 'text', 'eol=lf', 'crlf', 'crlf=input']) {
    test(`${attribute} is ineligible even when the normalized tree is unchanged`, () => {
      save();
      mkdirSync(join(repo, '.git/info'), { recursive: true });
      writeFileSync(join(repo, '.git/info/attributes'), `src/b.ts ${attribute}\n`);
      const value = { ...finding(), snapshot_covered_paths: finding().evidence_paths };
      expect(save(value).findings[0].snapshot_covered_paths).not.toContain('src/b.ts');
      expect(check(value).reusable).toBe(false);
    });
  }

  test('configured line conversion fails closed, including supplied coverage', () => {
    save();
    git('config', 'core.autocrlf', 'input');
    expect(check().reusable).toBe(false);
    expect(save({ ...finding(), snapshot_covered_paths: finding().evidence_paths } as any)
      .findings[0].snapshot_covered_paths).toEqual([]);
  });

  test('ignored tracked files, symlinks and missing paths cannot receive coverage', () => {
    writeFileSync(join(repo, '.git/info/exclude'), 'src/b.ts\n');
    expect(save().findings[0].snapshot_covered_paths).toEqual(['src/a.ts']);
    writeFileSync(join(repo, '.git/info/exclude'), '');
    rmSync(join(repo, 'src/b.ts'));
    symlinkSync('a.ts', join(repo, 'src/b.ts'));
    expect(save().findings[0].snapshot_covered_paths).toEqual(['src/a.ts']);
    rmSync(join(repo, 'src/b.ts'));
    expect(save().findings[0].snapshot_covered_paths).toEqual(['src/a.ts']);
  });

  test('a symlink ancestor does not read an external caller', () => {
    mkdirSync(join(fixture, 'outside'));
    writeFileSync(join(fixture, 'outside/caller.ts'), 'outside\n');
    symlinkSync(join(fixture, 'outside'), join(repo, 'external'));
    const value = { ...finding(), evidence_paths: ['src/a.ts', 'external/caller.ts'] };
    expect(save(value).findings[0].snapshot_covered_paths).toEqual(['src/a.ts']);
    expect(check(value).reusable).toBe(false);
  });

  test('literal spaces, Unicode and pathspec-looking names remain exact', () => {
    const paths = ['src/a.ts', 'src/é file.ts', 'src/[x].ts', 'src/:special.ts'];
    for (const path of paths.slice(1)) writeFileSync(join(repo, path), 'export {};\n');
    const value = { ...finding(), evidence_paths: paths };
    expect(save(value).findings[0].snapshot_covered_paths).toEqual(paths);
    expect(check(value).reusable).toBe(true);
  });

  test('inspection cannot execute fsmonitor hooks or mutate the current start', () => {
    save();
    const marker = join(fixture, 'hook-ran');
    const hook = join(fixture, 'monitor');
    writeFileSync(hook, `#!/bin/sh\ntouch '${marker}'\n`);
    chmodSync(hook, 0o700);
    git('config', 'core.fsmonitor', hook);
    const token = cli(['--start', 'review']);
    expect(check(finding(), token).reusable).toBe(true);
    expect(existsSync(marker)).toBe(false);
  });

  test('raw bytes must equal the captured blob, not merely an index entry', () => {
    const tree = save().wtree;
    writeFileSync(join(repo, 'src/b.ts'), 'export const changed = true;\n');
    expect(sharedLibsSnapshotCoverage(repo, tree, finding().evidence_paths, env)).toEqual(['src/a.ts']);
  });

  test('removing a transform cannot manufacture prior coverage', () => {
    writeFileSync(join(repo, '.git/info/attributes'), 'src/b.ts text\n');
    const row = save();
    expect(row.findings[0].snapshot_covered_paths).toEqual(['src/a.ts']);
    writeFileSync(join(repo, '.git/info/attributes'), '');
    const result = check();
    expect(result.snapshot.wtree).toBe(row.wtree);
    expect(result.snapshot.covered_paths).toEqual(finding().evidence_paths);
    expect(result.reusable).toBe(false);
  });

  test('legacy history without recorded coverage is never enough', () => {
    save();
    const logs = [...new Bun.Glob('**/*-reviews.jsonl').scanSync({ cwd: state, absolute: true })];
    expect(logs).toHaveLength(1);
    const row = JSON.parse(readFileSync(logs[0], 'utf8'));
    delete row.findings[0].snapshot_covered_paths;
    writeFileSync(logs[0], JSON.stringify(row) + '\n');
    expect(check().reusable).toBe(false);
  });

  test('legacy caller-supplied coverage cannot become logger-owned proof', () => {
    save();
    const file = [...new Bun.Glob('**/*-reviews.jsonl').scanSync({ cwd: state, absolute: true })][0];
    const row = JSON.parse(readFileSync(file, 'utf8'));
    expect(row.findings[0].snapshot_covered_paths).toEqual(finding().evidence_paths);
    delete row.shared_libs_coverage_version;
    writeFileSync(file, JSON.stringify(row) + '\n');
    expect(check().reusable).toBe(false);
    row.shared_libs_coverage_version = 999;
    writeFileSync(file, JSON.stringify(row) + '\n');
    expect(check().reusable).toBe(false);
  });

  test('the logger replaces supplied proof versions with its computed format', () => {
    const token = cli(['--start', 'review']);
    cli([JSON.stringify({ skill: 'review', status: 'clean', completed: true, converged: true,
      shared_libs_coverage_version: 999, findings: [finding()] }), '--finish', token]);
    expect(records().at(-1).shared_libs_coverage_version).toBe(1);
    expect(check().reusable).toBe(true);
  });

  test('gitlinks do not cover a nested repository caller', () => {
    const nested = join(repo, 'vendor');
    mkdirSync(nested);
    git('-C', nested, 'init', '-q');
    writeFileSync(join(nested, 'caller.ts'), 'export {};\n');
    git('-C', nested, 'add', '.');
    git('-C', nested, '-c', 'commit.gpgsign=false', 'commit', '-qm', 'nested');
    git('add', 'vendor');
    const value = { ...finding(), evidence_paths: ['src/a.ts', 'vendor/caller.ts'] };
    expect(save(value).findings[0].snapshot_covered_paths).toEqual(['src/a.ts']);
    expect(check(value).reusable).toBe(false);
  });

  test('partial clones are ineligible without fetching missing objects', () => {
    save();
    git('config', 'remote.origin.promisor', 'true');
    expect(check().reusable).toBe(false);
  });

  test('nonconverged passes and real defects cannot receive reusable advisory coverage', () => {
    const token = cli(['--start', 'review']);
    cli([JSON.stringify({ skill: 'review', status: 'issues_found', completed: true, converged: false,
      findings: [{ ...finding(), snapshot_covered_paths: finding().evidence_paths }] }), '--finish', token]);
    expect(records().at(-1).findings[0].snapshot_covered_paths).toEqual([]);
    expect(check().reusable).toBe(false);
    save();
    expect(check({ ...finding(), advisory: false }).reusable).toBe(false);
    expect(check({ ...finding(), severity: 'CRITICAL' }).reusable).toBe(false);
  });
});
